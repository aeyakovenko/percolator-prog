# Scope C: Carry And Observation Conformance

Base: `d346cc90cc4a473b9b55664dc15ab8af7661ceef`, the fetched tip of
`origin/codex/astra-open-holdout-ledger-20260912` when this work began.
Branch: `codex/astra-scope-c-carry-observations-20260913`.
Isolated clone: `/tmp/percolator-scope-c-20260913`. Neither protected checkout
was edited; all edits and build outputs belong to this clone.

No production, engine-pin, wire, fixture-program or machine-status changes.
Rows 425 and 426 remain OPEN. No implementation conformance mismatch was found
in the bounded histories below; there is no production red/green claim.

## Existing Coverage And Increment

Read `scripts/loop.md`, the invariant charter, coverage ledger and invariant
README, and the relevant INV-020/024/038/041/045/052/053/054/056/061/071/072/
081/085/086/088 test owners and their related carry/observation children.

| Existing family | Distinction from this increment |
| --- | --- |
| Scope H target arrival | Arrival, plateau, same-target renewal and a restarted anchor; bilateral reductions. C adds three nonzero resets per asset after K and anchor movement with repeated CPI/bilateral handoffs. |
| Overlapping funding checkpoints | Three nonzero resets before K movement and two funding activations. C resets after movement using a noninitial anchor; funding is zero. |
| Generated fractional K/F and funding retry | Fractional-owner cadence, fixed targets or one pending checkpoint. C supplies a different repeated target/publication/transport history, without extending the fractional-owner oracle. |
| Scope O current Hybrid recipient | Solvent active recipient, single-provider feed, four trade routes and missing-tail/stale-report prefixes. C carries a two-provider composite recipient into a second liquidation, including CPI admission and exit. |
| Existing composite recipient-target | Four bilateral controls with cross-epoch rejection and exact exit. C reuses this fixture and oracle, adds missing-tail prefixes around provider completion, account certification, liquidation and reduction plus actual SPL payout, and compares CPI controls. |
| Scope V Hybrid reward lineage | Independently moving target/recipient feeds, recipient PnL and reduction order. C's recipient is underwater and becomes the next target; its two-provider composition and CPI route are distinct. |
| Scope T funded-role epochs | Oracle/backing/insurance role returns and current observations over funded domains. C does not repeat same-price role probes or add authority succession. |
| INV-020 renewed/active/CPI/reward-payout children | Repeated rewards or a flat/solvent recipient. C binds rollback and retry to the composite recipient's second liquidation and full recipient withdrawal. |

The exact old composite selector remains available and passes its original four
worlds through the shared helper. The new selector includes those four controls
explicitly. Only the other twelve combinations are new: four interrupted
bilateral, four uninterrupted CPI and four
interrupted CPI worlds.

## Row 425 Guarantee

`inv_045_moving_reset_routes.rs` is a child of Scope H and reuses its bigint
episode book, complete Account rollback executor and resolved payout fixture.
System/SPL/ATA/wrapper instructions create and fund four owners. Asset-1
exposure is reversed through a public matched trade at the entry price so
each owner's two PnL directions align. Mint authority is revoked.

The sixteen worlds cross both price directions, single/two-leg-batch reductions,
bilateral-only versus bilateral/CPI/bilateral/CPI schedules, and grouped versus
one-slot accrual. Accrual cadence is coupled to observation/publication order and
whether reduction precedes or follows the publication. These axes are not
independently exhausted. Each world reaches a one-tick target by slot 5, creating
a noninitial cap anchor, then changes the target at slots 5/10/15/20. The last
three publications per asset consume a nonzero prepublication carry after real
price movement. Prices also accrue after the final reset, through slot 25.
No tested reduction changes the price profile or touches either passive owner.

The independent book prices an entire target episode with bigint
`anchor * cap_bps * elapsed / 10000`, retaining the exact remainder. It derives
K, each owner's still-unsettled value, OI and final entitlement from public
inputs. Every checked prefix reconciles individual capital/PnL/latent K with
that entitlement, plus fixed mint supply and custody. Complete Account rollback
includes compiled transaction keys, matcher context, owners, portfolios and
tokens, apart from the exact signature fee. Economic instructions retry with
fresh transaction envelopes. This is not retained-signed-envelope coverage.

Results: 16 worlds, 96 nonzero per-asset resets, 568 book checks, 224 exact
rollbacks and 64 exact resolved SPL payouts. Peak measured CU: 436627, ceiling
600000. The rollback count comprises 128 publications and 96 reductions;
terminal payout rollback is inherited coverage outside this increment and is
not counted here. The new selector does not measure transaction packet sizes.

The first development run used a matcher grant created before a bilateral
reduction; the later CPI correctly returned Unauthorized. Refreshing that
position-bound grant through public SetMatcherConfig, as the existing transport
fixture does, made the intended handoff valid. This was a fixture correction,
not a production mismatch. Configuration does not alter the checked price profile
or owner entitlement.

Limits: two assets, four solvent owners, monotone targets after arrival, integral
lots, unit ADL, zero funding/trading/maintenance fees, no source-credit consumption,
and explicit market accrual before reductions. The cap anchor changes once, then
remains fixed during the repeated resets. Arbitrary moving-anchor histories,
target reversals with prior positive-claim consumption, fractional K/F owners,
successful inline accrual, funding queues, other oracle modes, maximum shapes
and generic arithmetic/route equivalence remain open. Owner settlement and
terminal payout order are bounded fixture schedules, not a universal liveness proof.

## Row 426 Guarantee

`inv_020_reward_recipient_liquidation.rs` shares its existing public four-owner
fixture and independent health/stock/penalty oracles with the new selector.
A recipient initially receives 2925 atoms from an 8778-atom first penalty while
its own composite loss remains unsettled. Coherent two-provider evidence and
account recertification recognize its 50000-atom loss. The recipient then becomes
the next liquidation target, paying a 3564-atom penalty whose next recipient gets
1187 atoms. Its exact residual owner entitlement is
`120000 + 2925 - 50000 - 3564 = 69361` atoms.

The sixteen worlds cross CPI/bilateral, clean/interrupted, single/one-leg-batch
exit, and selected/empty hints for the second liquidation after full observation.
CPI admission uses the recipient as taker and its peer as delegated maker; the
bilateral fixture opens the same signed positions in the reverse participant
orientation. Both CPI admission shapes are exercised; bilateral admission retains
the existing single-trade setup. Exit uses the chosen single/batch transport.

Each interrupted world omits one provider while the hint still declares two:
before recipient catchup, after a completed peer observation, after recipient
certification, after second-target liquidation, and after owner reduction plus
actual SPL withdrawal. Error index and program-success logs prove those prefixes
executed. The payout case additionally requires SPL success. Complete
`Option<Account>` frames cover tracked and compiled keys, including Clock,
oracle reports, all four portfolios/tokens, matcher context, lamports, metadata
and absence; only the exact runtime signature fee changes. The same economic
instructions then retry successfully, including the unchanged withdrawal intent.

The inherited cross-epoch provider rejections run before and after catchup.
Independent current-health, market stock, reservation and fixed-mint custody
censuses supplement exact recipient capital/PnL, target account frames, penalties,
reward recipients and domain budgets. Normalized final outcomes agree across all
sixteen worlds. This does not add an independent liquidation-quantity oracle:
penalty arithmetic consumes the executed quantity, as in the existing test.

Results: 16 worlds including four old controls, 72 exact rollbacks (32 existing
cross-epoch checks plus 40 missing-tail checks), eight restored SPL payout
prefixes, and sixteen 69361-atom recipient payouts. Peak measured CU: 396803,
ceiling 500000. Checked transactions are at most 1232 bytes. Setup/configuration
costs are not comprehensively included in either selector's CU maximum.

Limits: the initial and second liquidations occur in one bounded market-catchup
episode with fixed feeds and role holders. Funding, maintenance/trading fees,
provider succession, repeated recipient renewals, inverse asset/sign assignments,
maximum shapes and terminal disposition of the other three owners are outside
scope. Single-leg batches are transport evidence only. No-tail favorable actions
consume already committed complete current market evidence. Missing *declared*
tails reject structurally even when market evidence is current; these transaction
rollback checks do not prove discovery of an omitted unknown Hybrid observation,
or authorize favorable actions while an unobserved price remains unknown.

## Artifacts And Commands

Reused the fixed default-feature Scope W artifacts. A read-only comparison
against its checkout found no differences in `src`, `Cargo.toml`, `Cargo.lock`
or the matcher sources relative to this base. Engine pin:
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.

| Local artifact | SHA-256 |
| --- | --- |
| `/tmp/percolator-scope-c-20260913/target/deploy/percolator_prog.so` | `79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673` |
| `/tmp/percolator-scope-c-20260913/tests/fixtures/auth_matcher/target/deploy/auth_matcher.so` | `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93` |

Both were copied from the corresponding paths under
`/run/percolator-pr135-scope-w-20260913`. No SBF rebuild was needed. A private
5 GiB executable tmpfs at `target/host` holds a copied host dependency cache;
Cargo rebuilt both test targets from this clone. No other checkout's build cache
was written. Host build flags and exact selectors follow.

```sh
cd /tmp/percolator-scope-c-20260913
export CARGO_TARGET_DIR="$PWD/target/host" TMPDIR="$PWD/target/host"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so"
carry=inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::moving_reset_routes::v16_program_repeated_moving_target_resets_preserve_carry_across_matcher_handoffs
observations=inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::reward_recipient_liquidation::v16_program_composite_recipient_target_routes_restore_missing_evidence_prefixes_and_payout
cargo test --locked --offline --test v16_cu -- --exact --list "$carry" "$observations"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 "$carry" "$observations"
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_045_no_free_mark_movement::public_carry_order::target_arrival_entitlement::v16_program_target_arrival_plateau_and_restart_preserve_interleaved_owner_entitlement \
  inv_045_no_free_mark_movement::public_carry_order::funding_carry_entitlement::checkpoint_replacement::v16_program_overlapping_target_replacements_preserve_carry_funding_and_resolved_payouts \
  inv_045_no_free_mark_movement::v16_program_fractional_target_reversal_commutes_with_neutral_reduction \
  inv_052_split_merge_invariance::v16_program_target_change_resets_prior_price_movement_remainder \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::reward_recipient_liquidation::v16_program_reward_recipient_becomes_liquidation_target_after_composite_refresh \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::v16_program_cpi_active_keeper_observations_preserve_admission_and_payout \
  inv_020_authenticated_clock_slot_and_oracle_provenance::staged_action_observations::active_keeper_observations::cpi_keeper_observations::generated_current_hybrid::v16_program_generated_current_hybrid_recipient_routes_match_full_health_and_rollback \
  inv_045_no_free_mark_movement::trade_origin_catchup::authenticated_reward_handoff::hybrid_recipient_provenance::v16_program_dual_hybrid_reward_lineage_survives_recipient_routes_and_payout \
  inv_053_full_health_recertification_equivalence::v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand \
  inv_056_hints_are_discovery_only_favorable_actions_fully_refresh::v16_bpf_inv056_mixed_observations_preserve_full_refresh_trade_boundary
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check --oneline HEAD
git show --check --oneline HEAD^
sha256sum target/deploy/percolator_prog.so tests/fixtures/auth_matcher/target/deploy/auth_matcher.so
```

Validation: new selectors PASS 2/2 in 23.89s; the exact listing selects two tests.
Adjacent controls PASS 10/10 in 64.55s. The four metadata gates PASS 4/4.
Formatter, working/staged diff checks and both local commit show checks pass.
The metadata target emits existing dead-code warnings, and Cargo reports the
Solana 1.18 dependency future-compatibility warning. No full repository suite,
Kani proofs, fuzz campaign or generic invariant certification was run.

## Local Changes

Test commit: `f6d3f1819a63379e05dc149404c5063b3ab2265a`,
`test(invariants): cover moving carry resets and composite recipient retries`.
The following documentation commit records coverage and keeps both rows OPEN.

- `tests/invariants/cu/inv_045_moving_reset_routes.rs`: new bounded carry family.
- `tests/invariants/cu/inv_045_target_arrival_entitlement.rs`: child registration.
- `tests/invariants/cu/inv_020_reward_recipient_liquidation.rs`: shared fixture,
  CPI/missing-tail selector and exact rollback helper; original control retained.
- `tests/invariants/README.md`: scope entry and limits.
- `tests/invariants/coverage_reopenings.tsv`: comments only for rows 425/426.
- `tests/invariants/pr135_scope_c_carry_observation_conformance_20260913.md`:
  this comparison, provenance and validation record.
