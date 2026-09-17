# Terminal payout, reserve and entitlement audit, 2026-09-17

Base: local `origin/main`, `cd751b44347f7d3ce4c5ece349571530535b875a`.
Branch: `audit/terminal-payout-reserve-20260917`.
Worktree: `/dev/shm/percolator-terminal-payout-audit-20260917`.
Scope: INV-024/027/067/070 and only the 15 benchmark rows below.

**13 distinct selectors pass; seven existing witnesses fail.** Every named family
has a mounted public-route witness. No distinct missing public-interface LoF/DoS
case was established, so this increment adds documentation only. In particular,
row 417's reproduced receipt loss already has an independent discovery selector;
another reproduction would duplicate it. No production fix, new test, engine proof,
ledger edit or status change is claimed.

The invariant statements, public fixtures and assertions were reviewed under
[scripts/loop.md](../../scripts/loop.md). Open-finding/discovery/nonqualifying rows
were withheld comparison data, not generator inputs or specifications imported
from external issues/PR patches. Historical severity labels are not re-certified:
there is no exact-parent/fixed-head experiment in this documentation increment.

## Row map

C1-C15 identify selectors in the first CU command below, with C12 corrected and
executed separately. C16-C17 are the follow-up scan controls; S1-S3 are the stateful
command's selectors. Successful execution proves those exact selectors are mounted;
the failures also enter their named test bodies. Availability is not a green result.

| Rows | Witness, current result and assertion boundary |
| --- | --- |
| 237/287/288 | C1 **passes**: untrusted insurance withdrawal and resolve-policy rewrite reject with exact market/vault/destination preservation and zero attacker tokens. This shared negative control neither exercises malicious authorized administration nor proves reclaim-window/deadline safety. The historical nonqualifying rationale cannot by itself discharge the current bounded-admin requirement. |
| 283 | S1 **passes**: both reported-price routes compare dust/no-dust worlds, victim loss is zero, coalition loss and remaining vault residue are exactly one atom, supply agrees and terminal retries are quiescent. Eight generated seeds plus persisted regressions; fixed economic amounts and no slab-retirement claim. |
| 287/288 | In addition to C1's narrow authorization boundary, source review finds resolve-policy writes limited to Live mode, bounded delays and rejection after stale maturity in `handle_configure_permissionless_resolve`. This is not an executed adversarial-admin deadline proof. |
| 330 | C8 **fails** on backing withdrawal after insurance payment. It reaches zero portfolios, user payouts `[0,40000000,0]` and the exact 20,000,000-atom claim-free residual, but does not certify the final 380,000,000 provider payout/empty vault. |
| 372 | C2 **passes**: public two-episode Recovery preserves the previous 48,000 claim, clears the opposing zero-basis obligation and pays 1,048,000 through owner-signed CloseResolved. Live conversion rejection alone is not the oracle. |
| 373 | C3 **passes**: retained signed forfeit after the opposing exit keeps 99,000 PnL, at least the earlier 48,000 claim, and pays 1,099,000. This bounded prerequisite control does not establish all retained haircut histories. |
| 377 | C4 **passes**, mounted under INV-073: after asset-0 Recovery, provider principal and one remaining insurance atom exit; restart changes generation, fresh 17-atom insurance enters/exits with zero inherited spend, and a fresh bilateral trade returns both 600-atom principals. |
| 410/429 | C5 **passes**: administrator-directed wrong destinations reject, while 137 atoms each reach the separate backing owner and insurance beneficiary with exact ledger attribution. Reserves began at 401 each; this partial shutdown-cleanup witness does not prove full retirement. C15's earned-fee succession join **fails** its final control-sequence frame. |
| 413 | C6 **passes**: 21 accrued fee atoms per owner precede 100-versus-101 margin admission; standalone excess and excess after a successful fee/refresh prefix reject atomically, exact admission succeeds with bundled or separate public fee/refresh prefixes, then four owner payouts are exact. This does not certify implicit fee collection on every transport. |
| 434 | C7 **passes**: open/close/age/reopen retains 7 collected plus 14 pending fee atoms per owner. A fee/refresh/reopen bundle rejects 101-atom margin atomically and admits the exact 100-atom boundary. The successful path explicitly settles fees first. |
| 417 | S2 **fails**, losing the only remaining claimant's receipt before unrelated backing expires. C12 **passes** four worlds with two partial receipts, exact/late expiry, destination rotation, eight paid-prefix rollbacks and eight retained top-ups. C13 **passes** four native redemption/recreation worlds with eight top-ups, four custody rollbacks, 20 portfolio deletions and four slab closures. Multi-claimant green suffixes do not discharge S2's last-claimant failure. |
| 418 | C9/C10 **fail** before expiry/residue disposal, on the same control-sequence frame in native/classic fixtures. Their intended 12/2-world retirement products are not current passing evidence. C13 separately proves native receipt-derived rounding pays two atoms to the insurance beneficiary, unchanged native mint, exact rent/donation partition and tombstone; it is not fee/recredit booked-residue coverage. |
| 424 | C11 **fails** in the first expiry/payment/close rollback bundle before scanner rediscovery. C16 **passes** 54 deadline/cadence worlds, 546 commits and 748 rollbacks (492 scanner prefixes, 108 custody prefixes), peak 417,817 CU. It excludes insurance recredit/new earlier obligations. C17 **fails** the native final-close preview before post-preview SyncNative. |

## Entitlement joins and failures

S3 **passes** 48 solvent live-to-resolved worlds, 1,992 checked transactions,
48 live payouts and 240 terminal payouts/deletions. Its input-derived owner ledger
retains earlier payments across two orders of the four trade transports, changing
winners, payout partitions, claimant orders and capital/direct-PnL exits. It does
not compose those histories with S2's underfunded final receipt.

C14 **passes** both Live/Resolved maintenance orders: each owner bears its own
35-atom fee, user payouts are 965/137 and protocol fees are 70, with exact custody.
This proves the scoped senior-principal join, not arbitrary loss-stale admission.

| Failing selector | Exact stopping point and practical limit |
| --- | --- |
| S2 | [stateful INV-067:276](stateful/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs#L276): receipt `present=false`, winner `1250 -> 1250` versus required 1750; mint supply `4500000000 -> 4499999500`. Peak 208,227 CU. The trace reaches slab close, then the receipt assertion fails; later payout/supply/tombstone assertions are not reached. This is the already-mapped row 417 failure, not a new discovery. |
| C8 | [CU INV-067:1045](cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs#L1045): remaining backing withdrawal returns `InstructionError(2, Custom(19))` (`EngineStale`), 200,896 CU. Source captures authority epoch before insurance withdrawal and reuses it afterward. Final provider/vault checks are not reached. |
| C9/C10 | [shared progress fixture:278](cu/inv_073_terminal_progress_product.rs#L278): actual `authority_epoch=4`, expected 3; other sequence lanes agree. The insurance payment prefix advances the epoch before expiry, wallet-repair and residue-retirement suffixes. Neither full world count is certified. |
| C15 | [earnings succession:679](cu/inv_024_terminal_earnings_succession.rs#L679): actual authority epoch 4 versus expected 3 at the final insurance-payment check. Paid principal/fee succession and balance assertions precede the frame failure; final ledger checks remain unexecuted. |
| C11 | [shared transaction helper:68](cu/inv_071_terminal_prefix_recredit.rs#L68): third instruction of `[close, insurance payment, close]` returns `InstructionError(4, Custom(19))`, expected `Custom(21)` (`EngineLockActive`). The second close reuses the pre-payment epoch. The helper fails before checking this bundle's full rollback frame; committed rediscovery/payment/retirement suffixes remain unexecuted. |
| C17 | [native reclassification:364](cu/inv_070_terminal_native_reclassification.rs#L364): retained close simulation returns `InstructionError(2, Custom(19))`, 2,489 CU, after earlier insurance withdrawal. The close was built before that epoch-consuming debit; no successful preview or later SyncNative/close result is certified. |

Production [insurance withdrawal](../../src/v16_program.rs#L11050) advances the
authority epoch on debit; [epoch validation](../../src/v16_program.rs#L1817)
rejects old epochs with EngineStale. This explains the six non-S2 stopping points
at source level; no corrected-history execution or no-escape DoS is inferred.
Their expectations and all production/test files remain unchanged.

## Exact verification

Private host cache copied from
`/dev/shm/percolator-public-gap-20260916-c91e-host-target`; only host harnesses
were recompiled. Reused wrapper SBF matches source checkpoint
`cb236b5248c941ffcb33e1e6afe62801e8a19712` in `src`, `Cargo.lock` and matcher
sources. The sole manifest delta adds host `syn` parser features.
Engine: `94979ede7db934545e53a8f210dd063a9ea3ea63`.

SHA-256: wrapper `87011b683219d59bd5e3f328569bc675b7198d503532491d8f3b31341d54f776`;
fresh authenticated matcher `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.
External sibling matcher `/dev/shm/percolator-match/target/deploy/percolator_match.so`
hash `51f361c6fd00bdb91c685e98f081dea5a54665ef533a7f7b619916594aae6755`;
its source/build provenance was not re-established.

Commands from the worktree (exports spell out the same environment passed via
`env` during execution). Full logs:
`/dev/shm/percolator-terminal-payout-audit-20260917-logs/`.
No broad suite, metadata census, Kani or engine proof was run.

```bash
cp -a --reflink=auto /dev/shm/percolator-public-gap-20260916-c91e-host-target /dev/shm/percolator-terminal-payout-audit-20260917-host-target
export CARGO_TARGET_DIR=/dev/shm/percolator-terminal-payout-audit-20260917-host-target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export PERCOLATOR_FUZZ_SBF=/dev/shm/percolator-public-gap-20260916-c91e-sbf-target/deploy/percolator_prog.so
git diff --exit-code cb236b5248 HEAD -- src Cargo.lock tests/fixtures/auth_matcher tests/fixtures/hostile_matcher
git diff cb236b5248 HEAD -- Cargo.toml
cargo test --locked --offline --test v16_cu --test v16_program_stateful_fuzz --no-run
CARGO_TARGET_DIR=/dev/shm/percolator-terminal-payout-audit-20260917-matcher-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir tests/fixtures/auth_matcher/target/deploy -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_005_authority_incarnation_binding::v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_prior_claim_forfeit_prerequisite_matrix_preserves_withdrawable_value \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_retained_recovery_haircut_prerequisite_matrix_keeps_prior_claim_floor \
  inv_073_no_permanent_user_lock::v16_program_asset0_recovery_matrix_preserves_provider_withdraw_and_restart_progress \
  inv_024_attributed_quote_value_conservation::v16_program_shutdown_submitter_cannot_receive_foreign_reserve_cleanup \
  inv_027_protected_principal_seniority::v16_program_flat_first_admission_fee_prefix_is_atomic_and_entitled \
  inv_027_protected_principal_seniority::joint_admission_liabilities::v16_program_flat_reopen_fee_history_precedes_new_exposure \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_native_booked_residue_escheats_after_fee_recredit_completion \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_classic_booked_residue_burn_control_preserves_paid_claims \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_scan_recredit::v16_program_terminal_scan_rediscovers_earlier_insurance_after_later_expiry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::terminal_claim_late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::native_receipt_redemption::v16_program_native_receipt_redemption_preserves_topups_and_rounding_beneficiary \
  inv_027_protected_principal_seniority::maintenance_terminal_seniority::v16_program_maintenance_terminal_orders_pay_senior_principal_before_protocol_extraction \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::v16_program_terminal_earned_fee_succession_preserves_paid_prefix_and_insurance
cargo test --locked --offline --test v16_program_stateful_fuzz -- --exact --nocapture --test-threads=1 \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_reported_route_matrix_preserves_terminal_value_partition \
  inv_067_terminal_payout_completeness_and_exact_once_settlement::v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt \
  inv_024_attributed_quote_value_conservation::v16_program_live_payout_histories_preserve_entitlement_through_resolution
cargo test --locked --offline --test v16_cu inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::v16_program_retained_claim_identity_survives_late_expiry_recipient_rotation_and_atomic_retry -- --exact --nocapture
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::generated_prefix_actionability::v16_program_generated_scans_recompute_actionability_across_source_deadlines_and_prefixes \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_native_reclassification::v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus
git diff --check cd751b44
git diff --exit-code cd751b44 -- . ':!tests/invariants/README.md' ':!tests/invariants/terminal_payout_entitlement_audit_20260917.md'
git diff --cached --check
git show --format= --check HEAD
```

The initial C12 filter used the filename-derived module `terminal_claim_late_expiry`
and selected nothing; its mounted module is `late_expiry`. The separate corrected
command executes exactly one test. The unmatched filter is never counted as evidence.

| Log | Result |
| --- | --- |
| `build.log`, `matcher-build.log` | Exit 0; host build 34.33s, matcher build 6.76s; existing warnings only. A prior artifact-copy attempt found no cached matcher, so the matcher was built fresh before execution. |
| `cu.log` | Exit 101; 9 passed, 5 failed, 1,425 filtered; 12.44s. |
| `stateful.log` | Exit 101; 2 passed, 1 failed, 325 filtered; 114.25s. S1 uses default eight generated cases plus persisted regressions, with no fuzz environment overrides. |
| `receipt-identity.log` | Exit 0; 1 passed, 1,438 filtered; 4.47s. |
| `scan-controls.log` | Exit 101; 1 passed, 1 failed, 1,437 filtered; 64.97s. |

All 20 executed selectors are non-ignored; no repeated or zero-test result enters
the 13/7 total. Whitespace and documentation-only diff checks pass.

## Kept open

Row 417's last-claimant loss and row 424's general actionability invalidation stay
OPEN. Rows 410/413/418/429/434 retain their existing COVERED entries despite the
specific failing joins recorded here. INV-024/027 remain OPEN_EVIDENCE and
INV-067/070 remain REFUTED_CURRENT. Nonqualifying rows retain their ledger entries.

Unexecuted suffixes above, arbitrary receipt/stock reclassification histories,
malicious authorized deadline changes, and the full fee/admission/terminal product
are not discharged. Keeper funding, valid/reconstructible custody and authenticated
time remain fixture prerequisites; unsigned economic disposition does not imply
unsigned portfolio deletion or administrator-free slab retirement. No finding
severity or whole-invariant closure is promoted.
