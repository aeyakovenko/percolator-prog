# PR135 Scope X: native residue and quote-variant retirement

Base: freshly fetched `origin/codex/astra-open-holdout-ledger-20260912` at
`d134c64d788e49264ecc0187a75209053925f43a`. Branch:
`codex/pr135-scope-x-native-terminal-residue-disposition-20260913`. Worktree:
`/tmp/percolator-pr135-scope-x-20260913`.

Only this new worktree was edited. Shared Git administration was used to fetch,
create the worktree and commit. Neither protected checkout's files were edited.
Only the requested base's source, invariant documents and tests were consulted;
no external PR branches, diffs or tests were inspected. Rows 418/420/421/433 are
coverage labels, not specifications or evidence of a newly observed violation.

## Open coverage and distinction

The root README requires safe economic wind-down before administrative reclaim.
INV-070 requires exact residue classification and balanced final disposal;
INV-073/078 distinguish economic progress from administrative retirement.
All four selected reopening rows are OPEN, and their `open_findings.tsv`
entries are `missing`. The invariant statuses and proof classifications remain
unchanged: INV-070/073 are `REFUTED_CURRENT`; INV-018/021/025/069/077/078/081
are `OPEN_EVIDENCE`, with sampled, conditional evidence.

Scope L covers native unspent insurance, multisig custody redemption/recreation
and raw donations through retirement, without provider claims or expiry.
Scope N covers generated principal/earned-fee/insurance payment words and wallet
availability on classic/native primary rails. Its native expired-principal
endpoints retain exact raw stock and explicitly stop before retirement.
Existing INV-073 native provider redemption covers one principal claim and
prefunded custody. Existing dual-quote reserve progress pays two backing domains
and insurance across rails, without expiry, missing wallets or native raw
donations. INV-070 native reclassification covers a persisted scan and donation
sync timing with paid backing; INV-077 secondary completion covers native
secondary surplus beside SPL expiry without this unsigned reserve custody history.

The new combination is dual-quote reserve completion after paid native custody
has been redeemed and both reserve wallets have disappeared, with independent
raw native classification and, on the SPL-primary branch, surviving expired
principal through actual retirement. The shared dual-quote fixture is extracted
without changing its public funding sequence or existing test assertions.

Native-primary expired booked principal remains open. The current close code
routes nonzero retired booked stock through SPL burning; the base's native
coverage deliberately excludes that disposition. This increment does not execute
or certify native principal burning, establish a production failure, or change
the protocol's native-residue policy. No production change is included.

## Exact coverage

One new exact selector:

```text
inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement
```

The 36 histories cross native primary/secondary placement, 0/19 raw vault
lamports, SyncNative never/before prefix/after reserve completion, and present
versus missing wallets. Fresh principal is paid on both placements; expiry is
sampled only with SPL primary/native secondary, adding twelve histories.
The fixed public fixture funds provider domains with 401/307 atoms and insurance
with 67 atoms on primary custody. Secondary liquidity is a separate 997-atom
donation. Both quote mints have nine decimals; the nonnative mint has its mint
authority removed publicly. No Token-2022 quote is admitted.

All worlds pay native prefixes of 101/59/17 atoms with only the keeper signing.
In missing-wallet worlds, each beneficiary signs redemption of its paid native
custody, receiving its exact prefix plus rent, then voluntarily transfers its
entire SOL balance to the administrator. The operator also drains its wallet.
All three role keypairs are dropped before subsequent payouts, including in
wallet-present controls. Remaining wallets stay byte-identical thereafter.
The keeper recreates both native ATAs in their respective payment transactions.
Fresh tails pay domain 0 and insurance on the native rail and domain 1 on the
other rail. In expiry worlds, the clock reaches slot 100 and one administrative
CloseSlab normalizes domain 1's remaining 248 atoms before the insurance tail.

An input-derived ledger checks each claim and its per-rail paid prefix. Claim
rank decreases by exactly each payment, or by 248 at expiry. Zero user capital,
PnL, portfolios, source claims, liens, provider earnings and spent insurance are
checked, alongside the insurance budget, source reservations, role profile and
control sequences. Stock and encumbrance censuses and market shape validation
accompany every checked economic prefix.

Let `P[r]` be cumulative payouts on rail r. Physical custody before sync is
`[775 - P[0], 997 - P[1]]`; logical vault stock is `775 - P[0] - P[1]`.
Logical stock equals unpaid claims plus explicitly expired booked residue.
Secondary payments displace exactly `P[1]` primary atoms into external surplus.
Native vault lamports are exactly rent + physical native stock + the raw donation;
wrapped amount includes that donation only after SyncNative. Full expected SPL
Account images also subtract previously redeemed native prefixes from recipient
custody, so same-address recreation cannot reset cumulative entitlements.

Before each tail payment, the identical repair/payment prefix followed by an
unsigned administrative close rejects with `ExpectedSigner`. Completed wrapper
and ATA prefix logs are counted. Every compiled/tracked Account is restored,
including lamports, missing custody and previously paid value; only exact payer
signature fees are charged. The unchanged prefix then commits keeper-only.
Every instrumented transaction reconciles total tracked/compiled lamports minus
signature fees, asserts exact payer rent/donation costs, and frames all Accounts
outside its declared write set. Each transaction is at most 1,232 serialized bytes
and has a 300,000-CU runtime and measured ceiling.

Final CloseSlab burns exactly 0 or 248 SPL atoms. Both vaults close, both surplus
destinations receive their exact token amounts, and the administrator receives
market excess rent, both vault rents and any still-unsynced native lamports.
The slab has the exact typed tombstone and rent. Subsequent administrator-signed
native redemption accounts for the sweep plus destination rent and preserves
the tombstone. Both supply equations include paid/redeemed value, retained SPL
destination custody and burned principal explicitly. The closed vaults retain
zero residue; nonnative administrator custody and beneficiary custody remain
separately accounted for, rather than being described as globally destroyed.

## Row impact and limits

| Label | Increment | Remaining limit |
| --- | --- | --- |
| 418 | Native raw/synced surplus, paid custody recreation, quote-variant final closure and exact rent | Native-primary expired booked principal; generic token/lifecycle products |
| 420 | Two provider-domain prefixes and absent-wallet public tails; one SPL expiry disposition | Earnings, losses/recredit, arbitrary providers and native expired principal |
| 421 | Unsigned insurance tail after native redemption/recreation beside another claim's expiry | Spent/recredited insurance, arbitrary authority and lifecycle histories |
| 433 | Mixed public reserve completion and dual-vault close after missing-wallet repair | Generic reserve histories, active liabilities, earned fees and maximum shapes |

INV-018/021 receive token/native custody and rent evidence; INV-025 distinguishes
logical stock, donated liquidity and residue; INV-069/070 receive the bounded
expiry/retirement subset; INV-073/078 receive a finite public payment suffix with
separate administrative normalization/retirement; INV-077 measures this one-asset
continuation, not maximum shape; INV-081 receives checked successful public routes.
No row, invariant status, independent-discovery classification or generic oracle
is promoted. `open_findings.tsv` and `invariant_status.tsv` are unchanged.

Limits include one asset, two domains sharing one provider, fixed integral unspent
claims, no trades/portfolios/receipts/pending losses, no provider fees or ledgers,
Recovery, insurance consumption/recredit, multisig/frozen/delegated custody,
repeated disruption, arbitrary expiry schedules or dense maximum capacity.
The retained administrator resolves, normalizes expiry and retires the slab;
beneficiaries voluntarily redeem the prefix before dropping their keys. Keeper
rent funding and correct System/SPL/ATA execution are required. The native mint
genesis account and signer SOL use the existing LiteSVM fixture; all market and
custody economic transitions are public instructions, with no injected images.

## Validation

A fresh default-feature SBF was built in this worktree using platform-tools v1.52,
the locked engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`, and offline dependencies.
SHA-256: `49f49d1a8a7a3c20e4e3e09c635c4d5ce08925056030668b6838ccf7224800b0`.
No matcher artifact is needed. SPL/ATA artifacts come from installed LiteSVM
dependency fixtures. SBF intermediates were cleaned after preserving the artifact.
The private host target is a 4-GiB tmpfs mounted inside this worktree because the
shared root filesystem was exhausted. A transient `git status` index-write failure
cleared when shared free space returned; no other checkout or cache was cleaned.

The first host compile required two missing error-type imports; the first executed
new selector passed 1/1 in 15.73 seconds, with 36 histories, 204 unsigned payments,
36 repairs, 12 expiry normalizations, 96 complete rollbacks, 36 retirements and a
71,856-CU peak. After adding an explicit decreasing claim rank, the final selector
and three native/secondary controls passed 4/4 in 20.47 seconds. Final new-probe
peak: 72,675 CU, below 300,000. The other five adjacent selectors, including
Scopes L and N and the extracted fixture's original test, passed 5/5 in 82.68
seconds. All selectors are exact, with no ignored tests or zero-match passes.
CU variation between runs includes randomly generated account keys.

Commands below were run from this worktree. The four INV-079 metadata gates,
format check and Git unstaged/staged/committed whitespace checks pass. Existing
unused test-support warnings and Solana future-compatibility warnings remain.
No unfiltered suite, engine proof, production correction or vulnerable-pin
experiment was run.

```sh
cd /tmp/percolator-pr135-scope-x-20260913
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_TARGET_DIR=$PWD/target/sbf-build RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/target/deploy" -- --locked
sha256sum target/deploy/percolator_prog.so
cargo clean --target-dir target/sbf-build
mkdir -p target/host
sudo -n mount -t tmpfs -o size=4G tmpfs "$PWD/target/host"
export CARGO_TARGET_DIR=$PWD/target/host
export PERCOLATOR_FUZZ_SBF=$PWD/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_program_fuzz_regressions --no-run
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement -- --exact --list
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement \
  inv_073_no_permanent_user_lock::v16_program_absent_native_provider_redeemed_prefix_preserves_public_remainder_and_close \
  inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_insurance_partial_redemption_reaches_bounded_terminal_exit \
  inv_077_bounded_work_and_maximum_shape_compute::secondary_quote_completion::v16_program_secondary_quote_repair_after_expiry_has_atomic_bounded_disposition
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_073_no_permanent_user_lock::dual_quote_reserve_progress::v16_program_unsigned_dual_quote_reserves_preserve_domain_claims_and_terminal_surplus \
  inv_073_no_permanent_user_lock::v16_program_generated_reserve_wallet_absence_preserves_fee_claims_across_expiry \
  inv_077_bounded_work_and_maximum_shape_compute::native_insurance_exit::v16_program_native_multisig_insurance_recreation_preserves_unsigned_retirement \
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_native_reclassification::v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus \
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value
cargo test --locked --offline --test v16_program_fuzz_regressions -- --exact --nocapture --test-threads=1 \
  inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete \
  inv_079_public_reachability_evidence::v16_machine_invariant_status_is_authoritative_and_nonoverclaiming \
  inv_079_public_reachability_evidence::v16_invariant_audit_summary_matches_every_verdict_row \
  inv_079_public_reachability_evidence::v16_post_pr135_counterexamples_reopen_every_affected_invariant
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The new source, fixture extraction, README index, reopening comments and this
note are one tests/docs commit. The deployable SBF is retained under `target/deploy`;
private host intermediates and the temporary mount are cleaned after verification.
