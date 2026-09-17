# Row418 fee/loss/recredit conformance, 2026-09-17

Base: local `origin/main`, `58f32546e52de398d90d741ffc0706e6fb5d181f`.
Isolated worktree: `/dev/shm/astra-ultra-row418-recredit-conformance-20260917`.
Test/docs only; local commit, no push. Row425/426/427, production, Cargo,
fixtures, harness and status TSVs are outside the change.

## Distinct Coverage

The [health report](row418_regression_health_20260917.md) identifies the stale
Scope O fee/loss constructor: it repeats `CloseResolved` while Fresh backing
keeps a haircut receipt alive. This independent selector reuses only the public
Live earnings constructor and transaction/token-image oracles, and supplies its
own settlement and reserve assertions. Existing Scope O selectors remain unchanged
and are not re-certified. The [reserve audit](terminal_reserve_evidence_audit_20260917.md)
and [Scope O](astra_scope_o_native_booked_residue_cleanup_20260914.md) describe the
historical boundaries.

The new [selector](cu/inv_070_native_fee_recredit_conformance.rs) retains all
100,000 provider-backing atoms through expiry. Public authenticated marks earn
875 fee atoms (657 provider, 218 insurance) and spend 73 insurance atoms. Initial
resolved settlement pays the user 2,051,699 and retains a 5,074-atom receipt paid
to 5,073. At slot 100, a public `PermissionlessCrank` with the asset discovery
hint expires backing and pays the remaining atom. The receipt becomes finalized,
and owner-signed `ClosePortfolio` deletes both portfolios and returns their rent
to the slab. No forgone user claim is used to manufacture booked residue.

The separate one-atom source-principal entitlement then pays the provider.
Unsigned reserve payments recredit all 73 spent insurance atoms while all 657
earned-fee atoms remain protected, pay 176 available insurance atoms, pay fees
in 17/640 portions with exact lazy-ledger attribution, then pay the recovered 73.
The input-derived final partition is:

```text
initial quote = 52,502 + 2,000,000 + 100,000 + 31 = 2,152,533
user payouts = 0 + 2,051,700
provider payout = 1 source principal + 657 earned fees = 658
insurance claims paid = 31 initial + 218 earned = 249
native booked residue = 100,000 - 1 user top-up - 73 recredit = 99,926
insurance final custody = 249 + 99,926 = 100,175
administrator wSOL = 0
administrator SOL refund = slab lamports + vault rent - tombstone rent
```

The two one-atom amounts have different owners: the remaining source principal
is refunded to the provider; expiry supplies the user's one-atom receipt top-up.
Final native retirement pays only claim-free residue to canonical insurance
custody. The native mint Account, paid fee ledger and prior recipients are framed.
The administrator receives the exact rent refund and no quote tokens.

Four exact rollbacks cover a successful recredit/payment before an unsigned
suffix; successful fee-ledger initialization/payment before that suffix; a
redirected booked-residue destination; and successful full retirement before an
unsigned suffix. Successful wrapper-prefix logs, exact error/index, every
compiled/tracked Account and signature fees are checked. The same recredit, fee,
and retirement prefixes then commit. Separate bounded transactions retain the
existing 300,000-CU ceiling and 1,232-byte packet cap.

Stock/encumbrance censuses, shape validation, native token/lamport images, exact
rent preservation, source/principal/earnings/insurance partitions, debit-epoch
consumption and typed tombstone assertions accompany success. Reserve keys are
dropped before all reserve withdrawals and retirement; wallets and custody stay
present. Initial signer SOL and native mint genesis use existing LiteSVM setup;
all economic transitions use public System/SPL/ATA/wrapper instructions.

This is INV-024/070 conformance with a corrected independent settlement route,
not the prefunded-custody case or native receipt redemption. It covers one asset,
one retained-backing amount, one fee order and full fixed recredit. It does not
establish absent-admin retirement, arbitrary recovery, partial recredit, dual
quotes, custody disruption or maximum shapes. No invariant status or severity
claim is promoted.

## Reproduction

Private host/SBF targets were copied with `cp -a --reflink=auto` from
`/dev/shm/percolator-public-gap-20260916-c91e-{host,sbf}-target` to the same suffixes
on the isolated worktree path. Current default-feature SBF was rebuilt with
platform-tools v1.52, locked/offline dependencies and unchanged engine
`4db11a8cb0053815e23a35d3a7d3edc265d8d866`.
SBF SHA-256: `a17c5dfa31c081067bdb7bdaab0543e25cf5563cf727762c41b9287d78bc8628`.

```bash
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR=/dev/shm/astra-ultra-row418-recredit-conformance-20260917-host-target
export PERCOLATOR_FUZZ_SBF=/dev/shm/astra-ultra-row418-recredit-conformance-20260917-sbf-target/deploy/percolator_prog.so
env CARGO_TARGET_DIR=/dev/shm/astra-ultra-row418-recredit-conformance-20260917-sbf-target RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc CARGO_NET_OFFLINE=true cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir /dev/shm/astra-ultra-row418-recredit-conformance-20260917-sbf-target/deploy -- --locked
cargo test --locked --offline --test v16_cu inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::fee_recredit_conformance::v16_program_native_fee_loss_expiry_recredit_partitions_booked_residue -- --exact --nocapture
rustfmt --edition 2021 --config skip_children=true --check tests/invariants/cu/inv_070_native_booked_residue_cleanup.rs tests/invariants/cu/inv_070_native_fee_recredit_conformance.rs
git diff --check
git diff --exit-code 58f32546e52de398d90d741ffc0706e6fb5d181f -- src Cargo.toml Cargo.lock tests/fixtures ':(glob)**/*.tsv' tests/v16_cu.rs
git diff --exit-code 58f32546e52de398d90d741ffc0706e6fb5d181f -- . ':!tests/invariants/cu/inv_070_native_booked_residue_cleanup.rs' ':!tests/invariants/cu/inv_070_native_fee_recredit_conformance.rs' ':!tests/invariants/row418_recredit_conformance_20260917.md'
git diff --cached --check
git show --format= --check HEAD
```

Only the exact new selector ran, including diagnostic iterations; no direct
control or broader suite was necessary because existing helper behavior is
unchanged. An initial portfolio-close assertion was corrected to match the
wrapper's empty, zero-lamport, program-owned account. An exploratory combined
recredit/fee prefix exceeded the inherited CU ceiling (357,610 CU); it was split
into separately bounded rollback/retry transactions. Neither observation is a
production bug claim. Existing Solana future-compatibility warnings remain.
Logs are `/dev/shm/astra-ultra-row418-recredit-conformance-20260917-*.log`.

## Final Results

The rebuilt SBF and final exact selector pass: **1 passed, 0 failed, 1,474
filtered**, 1.09s; one public history, four exact rollbacks and one full retirement.

| Measured phase | Peak CU |
| --- | ---: |
| Resolved settlement, expiry and portfolio deletion | 256,917 |
| Source principal, recredit, earned fees and insurance | 224,922 |
| Redirect rejection, retirement rollback and successful close | 40,948 |

Every instrumented transaction is below 300,000 CU. Initial Live construction,
trading and authenticated loss marks are excluded from these phase peaks. CU
varies with randomized fixture keys. Targeted rustfmt, whitespace, protected-path
and three-file scope checks pass; no ignored or zero-match run is counted.
