# Astra Scope B: native recredit custody

Base: freshly fetched `origin/codex/astra-open-holdout-ledger-20260912` at
`d346cc90cc4a473b9b55664dc15ab8af7661ceef`. Isolated local fork:
`/tmp/percolator-astra-b.0nnqE5/worktree`, branch
`codex/astra-scope-b-terminal-20260913`. The fork shares read-only base objects;
fetch, edits, commits and build outputs use its own Git administration and files.
Neither `/home/anatoly/percolator-prog` nor `/tmp/percolator-astra-watch.Cb2E7d`
was edited. Only the requested base's source/tests/docs were used for coverage
selection. No external PR source or alternate branch was consulted.

## Distinct Coverage

INV-073 owns the new public selector:

```text
inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::native_recredit_custody::v16_program_recredited_insurance_recreates_native_custody_without_role_signatures
```

The review included `scripts/loop.md`, `INVARIANTS.md`, the invariant README,
reopening comments, and relevant INV-018/021/024/025/027/063/067/069/070/071/073/
078/081/082 owners and terminal tests. The closest existing compositions are:

| Existing family | Boundary extended here |
| --- | --- |
| Scope X native residue disposition | Unspent reserves; no insurance consumption/recredit or preauthorized custody closer |
| Scope N generated reserve wallets | Earned reserve payment words and expiry on one quote rail; no insurance recredit |
| Scope L native multisig | Unspent insurance and quorum-signed native redemption; no recovered claim |
| INV-073 recredited insurance quote rails | Classic SPL on both rails, present custody, no native rent/donation/redemption |
| INV-073 missing insurance wallet recredit | One classic rail and initial ATA repair; no native custody or cross-rail paid prefix |
| INV-070 custody program recreation | Recreated wrong-program surplus custody, not recovered reserve entitlement |

The new composition starts from the existing public spent-insurance fixture and
crosses both selected assets, zero/one surviving insurance atom, classic-first/
native-first payment and never-sync/sync-before-repair schedules: 16 histories.
The new native branch uses the existing public native genesis fixture and public
mint configuration to select fixed-supply, nine-decimal classic SPL primary and
native secondary. The original fixture's classic branches retain their settings.

Three traders deposit 1,000/100/137 primary atoms, insurance supplies 100/101 and
the provider supplies 307. Public marks, trading and resolved loss settlement
consume 100 insurance atoms and pay users exactly 1,200/0/137. Both insurance
wallets and primary beneficiary custody disappear before trading; beneficiary,
operator and provider keypairs are then dropped. User payouts are permissionless
after the configured timeout. Earlier empty portfolio deletion uses owner keys.
Administrative expiry normalization exposes exactly 100 atoms of recredit and
leaves 207 atoms for final SPL retirement.

Before key departure, the beneficiary appoints the keeper as SPL close authority
on its old native ATA. That ATA contains 17 unsynced donated lamports, no claim
payment. Secondary vault liquidity is 211 wrapped atoms plus 19 unsynced lamports.
These independent donations create no engine stock or reserve entitlement.
Unsigned wrapper payouts require unencumbered destinations. The preauthorized
keeper therefore closes old native custody to the beneficiary, recreating the
absent System wallet with exactly custody rent + 17 lamports, then recreates the
same ATA without a close authority and pays 23 recovered atoms. A later payment
adds 31 native atoms. The classic rail pays the other 46/47 atoms either before
or between those native payments, with keeper-funded primary ATA recreation.

Every claim payment lowers input-derived unpaid rank by exactly its amount.
Full decoded market equality checks the one-time recredit and shared debit;
complete expected token Account images, authority profiles/sequences, mint
frames, raw custody, stock and reservation censuses and market shape accompany
each committed payment transaction. Fresh expected ATA bytes include cleared
old authority payloads. The operator wallet remains absent; the provider wallet
is framed. The recreated beneficiary wallet receives only the old rent/donation,
and neither recreated custody nor independent native liquidity resets its claim.

Before each of the three payments, an identical repair/sync/payment prefix plus
unsigned administrative close returns `ExpectedSigner`. Successful wrapper/ATA
prefix logs are counted. Every compiled/tracked Account rolls back exactly,
including wallet absence, prior custody authority, lamports, mint, market and
previous payments; only calculated signature fees are charged. The unchanged
instruction prefix then commits with only the keeper signing. Successful calls
frame all undeclared changes and reconcile tracked/compiled lamports less exact
signature fees; new ATA rent is charged exactly. All new transactions assert a
1,232-byte packet limit and a 300,000-CU runtime/measured ceiling.

After three payments, logical stock is exactly 207 and all insurance entitlement
is zero. Native payment has displaced exactly 54 primary atoms into raw surplus.
One signed `CloseSlab` burns 207 primary atoms, sweeps those 54 to administrator
custody, disposes both vaults and leaves the exact typed tombstone and rent.
Native surplus is 157 wrapped atoms plus the 19-lamport vault donation: synchronized
donations travel with the token sweep; unsynced donations travel with vault close.
Administrator-signed native surplus redemption reconciles the same total in SOL
in both schedules. Beneficiary custody retains exactly 46/47 classic and 54 native
atoms, separate from the recreated wallet's old rent/donation.

## Limits And Rows

| Row | Increment and remaining scope |
| --- | --- |
| 418 | Native secondary recredit/repair and exact terminal disposition; native-primary booked retirement and general quote variants remain open |
| 420 | Existing absent-provider expiry setup reused; no new provider claim/earnings coverage |
| 421 | Recovered insurance crosses native custody recreation and quote rails with no insurance role signatures; general consumption/recredit histories remain open |
| 433 | Reserve payment plus custody/wallet recreation and exact final close; arbitrary reserve histories remain open |

All four rows stay OPEN. `open_findings.tsv`, `invariant_status.tsv`, findings,
proof classifications, production, dependencies and engine pin are unchanged.
No implementation conformance mismatch was observed and no production fix or
red/green production experiment is claimed. INV-018/021/024/025 receive custody,
rent and attribution evidence; INV-063/067/069 reuse the bounded expiry/user
settlement history; INV-070/071/078/081/082 receive this finite terminal suffix.
This is not a generic reachability proof, maximum-shape result or new seniority
oracle. Active user claims at repair, partial recredit, earned reserves, optional
ledgers, authority succession, Recovery, arbitrary donation/expiry schedules,
frozen/multisig custody and Token-2022 remain outside the increment.

The keeper needs rent and its previously granted custody-close authority. The
role keys remain unavailable even after the wallet reappears. Final claim wSOL
remains in beneficiary custody; beneficiary-free redemption of that paid claim
is not demonstrated. The retained administrator normalizes expiry and retires
the slab. These administrative steps are distinct from unsigned reserve payments.
The existing LiteSVM fixture supplies signer SOL and native mint genesis only;
all market and custody economic transitions use public instructions.

## Artifact And Validation

The unchanged production artifact was copied from
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so` to
`/tmp/percolator-astra-b.0nnqE5/worktree/target/deploy/percolator_prog.so`.
Both have SHA-256:
`79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
Scope W records its default-feature, platform-tools v1.52 build against locked
engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`.
`git diff 0dbac7d2 HEAD -- src Cargo.toml Cargo.lock` is empty at the base, binding
that corrected build's production sources to this fork. No SBF rebuild or matcher
artifact is needed. SPL/ATA binaries use installed LiteSVM dependency fixtures.
A private 6-GiB executable tmpfs at `target/host` holds host intermediates because
the root and shared-memory filesystems were nearly full; no shared cache is edited.

New selector: PASS 1/1, 16 histories in 9.22s, no ignored tests. It checks 48 new
unsigned payments, 48 complete Account rollbacks, 32 ATA repairs, 16 wallet
recreations and 16 exact slab/vault retirements plus native surplus redemptions.
New suffix peak: 174,000 CU / 300,000. The reused instrumented terminal path peaks
at 224,407 CU; each of the three user portfolios takes one economic payout call
within the existing eight-call bound. Fixture initialization/trading CU is not
aggregated into this reported peak. Development runs corrected native fixture
capacity, reuse of its existing native vault, and expected fresh ATA bytes; they
did not identify a production mismatch.

All seven nearby exact selectors pass in 99.95s, including Scopes X/N/L, both
adjacent insurance-recredit families, original dual-quote reserves and native
roundtrip. The four metadata gates pass in 0.01s. `cargo fmt --all -- --check`
passes. All selectors select real tests, with no ignored or zero-match passes.
Existing unused test-support warnings and the Solana dependency future-compatibility
warning remain. No unfiltered suite or engine proof is claimed.

Exact validation commands from the isolated fork:

```sh
export CARGO_TARGET_DIR=/tmp/percolator-astra-b.0nnqE5/worktree/target/host
export TMPDIR=/tmp/percolator-astra-b.0nnqE5/worktree/target/host
export PERCOLATOR_FUZZ_SBF=/tmp/percolator-astra-b.0nnqE5/worktree/target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::native_recredit_custody::v16_program_recredited_insurance_recreates_native_custody_without_role_signatures -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::recredited_insurance_quote_rails::v16_program_recredited_insurance_switches_quote_rails_without_operator_signatures \
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::missing_insurance_wallet_recredit::v16_program_recredited_insurance_reaches_terminal_exit_without_wallets_or_signatures \
  inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement \
  inv_073_no_permanent_user_lock::dual_quote_reserve_progress::v16_program_unsigned_dual_quote_reserves_preserve_domain_claims_and_terminal_surplus \
  inv_073_no_permanent_user_lock::v16_program_generated_reserve_wallet_absence_preserves_fee_claims_across_expiry \
  inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_multisig_insurance_recreation_preserves_unsigned_retirement \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --check HEAD
```
