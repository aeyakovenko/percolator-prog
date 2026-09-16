# Row 411: Retained Route Consent With Existing Open Interest

One new test is mounted below INV-014's `retained_single_cpi_policy_history`:
`retained_oi_mark_consent::v16_retained_route_switch_caps_mark_fees_on_existing_open_interest`.
Four public LiteSVM histories cross opening direction and single/batch no-CPI
opening before retained batch-CPI continuation on the same owner pair and asset.
**Row 411 stays OPEN; INV-014 stays `REFUTED_CURRENT`; all TSVs are unchanged.**

## Distinct Obligation

A retained batch taker atom ceiling includes the fee for moving the mark of an
already-open position after a base-policy change. Paying for the old bilateral
fill does not authorize the next debit or prepay that movement. A rejection must
restore SPL custody, owner balances, position episodes, matcher requests and
oracle state, including when the rejected suffix follows a successful paid fill.

| Existing product | What this adds |
| --- | --- |
| `retained_mark_fee_cap::v16_program_retained_batch_atom_cap_includes_computed_mark_fees` | That starts with empty OI and two assets. Here a committed paid direct fill creates OI much larger than the new fill, and a failed suffix rolls back a successfully staged mark. |
| `retained_mixed_route_fees::v16_retained_mixed_route_fee_budgets_survive_bilateral_revocation_and_renewal` | That changes routes at a fixed manual mark. Here elapsed EWMA movement imposes a new, independently priced existing-position externality after signing. |
| `retained_policy_route_budgets::v16_retained_policy_route_budgets_bound_each_committed_prefix` | That composes heterogeneous base-fee/slippage budgets without trade-driven marks. This distinguishes budgeted opening/base fees from unbudgeted dynamic movement fees. |
| `retained_round_trip_fee_consent::v16_retained_single_cpi_round_trip_cannot_pool_instruction_fee_consent` | That rejects pooling single-CPI base-rate consent. This guards an atom cap on a much larger computed movement charge with an existing exposure. |

The fixture, signing, complete Account frames and independent stock/encumbrance
censuses are reused. There is exactly one new `#[test]`; shared helpers are not
changed. This is bounded composition evidence, not a new engine fee proof.

## Public Trace and Oracle

1. System/ATA/SPL/wrapper instructions create the market, two independently owned
   portfolios and deposits of 100,003 and 200,007 atoms. The first owner retains
   113 wallet atoms. Mint authority is revoked at supply 300,123.
2. Configure EWMA on the empty asset at slot zero, price 100, halflife one and
   zero minimum-fee attenuation. A bilateral single or one-leg batch trade opens
   `255 * POS_SCALE + POS_SCALE / 2 + 1` quantity at 19 bps. Each owner pays 49
   atoms, all credited to base-fee domain budgets. Public LP renewal restores the
   grant that bilateral execution disabled; the base-rate grant cap is 137 bps.
3. The authenticated matcher is configured for 1,000-bps bid/ask spreads. Retain
   three signed deposit-plus-batch transactions for an additional
   `25 * POS_SCALE + POS_SCALE / 3 + 1` quantity: one atom short, exact with a
   policy suffix, and exact without the suffix. The LP does not sign them.
   All three simulate successfully at slot zero and leave complete Accounts
   unchanged. Their signatures and serialized envelopes are preserved.
4. Publicly raise the base policy to 37 bps and advance authenticated Clock to
   slot one. This changes neither the retained wire bytes nor the live grant.
   Accepted prints are 90/110 and the candidate marks are 95/105. Existing
   one-side notional is `ceil(quantity * 100 / POS_SCALE) = 25,551`, greater than
   the new notional, 2,281/2,787. The independent movement requirement is
   `ceil(2 * 25,551 * 500 / 10,000) = 2,556` atoms across both owners.
5. Independently enumerate integral fee rates to find the first whose rounded
   two-sided fee pays both the 37-bps base fee and the movement requirement.
   No production fee/quote/mark helper supplies this oracle. The exact charge is
   1,287/1,289 per owner: 9/11 base plus 1,278 movement. Even the short cap exceeds
   the fee for empty OI plus the previously paid 49 atoms, so the old empty-OI
   case cannot explain this rejection.
6. The one-atom-short envelope rejects `InvalidInstruction` at instruction 3,
   after a successful 113-atom SPL deposit and matcher call. The exact-cap
   envelope with the now-stale policy suffix rejects `EngineStale` at instruction
   4, after the complete deposit and batch fill. Full Account rollback includes
   market/profile, portfolios, matcher context, SPL accounts, mint, sequences,
   ownership and lamports; the payer loses only its exact signature fee.
7. The independently retained exact-cap envelope commits. Each portfolio epoch
   and the matcher request sequence advance once; the grant sequence is stable.
   The paid target becomes 95/105 while the effective price remains 100. Both
   owners retain zero PnL and fee credits. Custody is 300,123. Total insurance is
   2,672/2,676, base-fee domain budgets total 116/120, and exactly 2,556 movement
   atoms remain unbudgeted. Both stock and encumbrance censuses pass.

All economic state comes from public instructions. Only program loading, signer
SOL funding and authenticated Clock advancement use harness environment APIs.
No program-owned account image is injected or mutated. Failed attempts are not
resubmitted under an already-processed signature: the succeeding alternative was
independently signed before the policy update.

Development probes first tried oracle reconfiguration after the opening. Both
attempts correctly hit `EngineLockActive`, even after using the current generation.
The retained test configures EWMA while empty and changes only base policy after
signing. Those setup failures are not evidence of a production bug.

## Isolation and Commands

- Source clone: `/tmp/percolator-astra-invariant-cycle-20260915-run`.
- Fetched origin branch: `codex/astra-invariant-cycle-20260915`.
- Base: `e4dc038f84845da1542521cd1913ce1b7e4abaa7`.
- Private clone: `/dev/shm/percolator-inv014-route-consent-20260916`.
- Private host cache copied without hardlinks from the Lane 25 target.
- Logs: `/dev/shm/percolator-inv014-route-consent-20260916-logs`.
- Reused SBF SHA-256: `e5f4c68d1e01b81e85a38019a93facc0ab0ad3dd9619d3dfdf536545af11530f`.
- Reused matcher: `/dev/shm/lane24-20260916-matcher/deploy/auth_matcher.so`, SHA-256
  `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
- Engine pin: `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

The clone's ignored matcher `target` symlink points at that existing build;
no tracked fixture file changes. `/home/anatoly/percolator-prog` is not edited.
Only the child, three-line parent mount, README entry and this report change.

The commands below use the same environment as each logged `env ... cargo`
invocation. All selectors use `--exact`.

```sh
export PERCOLATOR_FUZZ_SBF=/dev/shm/lane24-20260916-sbf/deploy/percolator_prog.so
export CARGO_TARGET_DIR=/dev/shm/percolator-inv014-route-consent-20260916-target
export TMPDIR=/dev/shm/percolator-inv014-route-consent-20260916-tmp
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::retained_oi_mark_consent::v16_retained_route_switch_caps_mark_fees_on_existing_open_interest \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::v16_retained_single_cpi_fee_consent_survives_policy_detours_and_funded_rollback \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::v16_retained_cpi_and_direct_policy_histories_differ_only_by_explicit_route_fees \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::retained_mixed_route_fees::v16_retained_mixed_route_fee_budgets_survive_bilateral_revocation_and_renewal \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_single_cpi_policy_history::retained_policy_route_budgets::v16_retained_policy_route_budgets_bound_each_committed_prefix

cargo test --locked --offline --test v16_program_stateful_fuzz \
  inv_014_delayed_policy_and_policy_epoch_safety::retained_mark_fee_cap::v16_program_retained_batch_atom_cap_includes_computed_mark_fees \
  -- --exact --nocapture --test-threads=1

cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_special_verification_method_registry_matches_charter \
  inv_079_public_reachability_evidence::v16_program_invariant_harnesses_are_test_free_roots \
  inv_079_public_reachability_evidence::v16_every_public_trace_consumer_validates_reachability_evidence \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant

rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_014_retained_single_cpi_policy_history.rs tests/invariants/cu/inv_014_retained_oi_mark_consent.rs
git diff --check
git diff --cached --check
git diff --exit-code e4dc038f84845da1542521cd1913ce1b7e4abaa7 -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)tests/invariants/*.tsv' ':(glob)tests/invariants/**/*.tsv'
git show --format= --check HEAD
```

## Results

| Validation | Result |
| --- | --- |
| New selector plus four adjacent CU controls | PASS 5/5, 28.08 seconds, `cu-final.log` |
| Existing retained computed mark-fee control | PASS 1/1, eight worlds, 3.95 seconds, `mark-control.log` |
| INV-079 metadata guards | PASS 7/7, 0.15 seconds, `metadata.log` |
| Both touched Rust files, focused rustfmt | PASS |
| Working/staged whitespace and committed patch checks | PASS |
| Protected-path diff against the fetched base | Empty; PASS |

The new selector contributes four worlds, twelve pre-policy simulations, eight
complete Account rollback checks and four exact-cap continuations. Its final
measured CU peaks are:

| Phase | CU |
| --- | ---: |
| Bilateral opening | 138,199 |
| Pre-policy simulation | 264,290 |
| Oracle configuration/base-policy update | 4,323 |
| Rejected continuation/suffix | 277,024 |
| Committed exact-cap continuation | 274,672 |

Every measured transaction is bounded by the shared 500,000-CU assertion; the
signed transaction limit is slightly lower to distinguish retained envelopes.
An earlier successful development run peaked at 290,036 CU. Random keys/PDA
derivation vary measured costs. Common account creation, funding, grant renewal
and matcher configuration are not included in this table. No whole-suite or
maximum-shape result is claimed. Existing dead-code/future-compatibility build
warnings remain; the selected tests have no failures.

## Evidence Limits

This product covers one asset, classic SPL, fully funded exact fills, two owners,
one policy increase and one elapsed EWMA slot. It ends with open positions and
the paid target still pending. It does not add catchup/payout evidence, single-CPI
total-atom bounds, LP-specific dynamic-fee bounds, partial fills, backing fees,
maintenance, liquidation, native quote, authority succession or arbitrary policy
histories. The 137-bps standing grant bounds base policy; it is not presented as
a total dynamic-fee ceiling. No real current bug was found in this product.
