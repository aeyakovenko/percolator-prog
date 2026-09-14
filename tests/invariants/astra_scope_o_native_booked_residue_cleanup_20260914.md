# Astra Scope O: native booked-residue cleanup

Base: `origin/codex/astra-open-holdout-ledger-20260912` at
`e527392517542b10a64d0ef303f346b0898509ae`. Fresh isolated worktree:
`/tmp/percolator-pr135-scope-o.HE46N2`. Neither protected checkout's working
files were edited. Shared Git administration was used only for worktree creation
and the local commit. All edits, logs and build intermediates belong to this
worktree. No external PR implementation or alternative branch was consulted.

## Coverage boundary

The initial review read Scope H's terminal audit, the invariant README sections
for H/B/native disposition, Scope B and Scope X audits, `scripts/loop.md`,
`INVARIANTS.md`, the wrapper and the pinned engine/SPL implementations.

| Existing owner | Existing boundary | Scope O addition |
| --- | --- | --- |
| Scope H terminal progress | 1,152 fee/loss/recredit histories; 192 native endpoints stop with 28 booked atoms | Actual retirement after one representative completion word, with 1/28 booked native atoms |
| Scope B native recredit custody | Native secondary payouts; final 207-atom booked retirement uses classic SPL | Native-primary booked residue is disposed separately from already paid insurance |
| Scope X native disposition | Native donations/sync and dual custody; native-primary booked retirement excluded | Donation/sync timing beside a nonzero native booked stock and actual slab close |
| INV-070 native reclassification and roundtrip | Exact native surplus/rent with zero booked native residue | Canonical insurance escheat and complete close/repair rollback at nonzero booked stock |

No H selector was copied or expanded. Scope O is a child of its existing owner
solely to reuse the public fee/loss constructor, input-owned `Book`, full Account
`Frames`, token-image oracle and transaction helper. Scope H's original exact
selectors and declared exclusions remain unchanged.

## Conformance correction

INV-070 requires claim-free vault residue to have an explicit balanced disposition
and then permit final `CloseSlab`. INV-069 permits normalization only after real
obligations are exhausted. The pinned engine's terminal retirement returns the
claim-free booked amount and debits logical vault/insurance stock. The original
wrapper routes that nonzero amount through SPL `Burn`, while the pinned SPL
processor rejects native token burns. The current PR135 pre-fix SBF reproduces
this as a public terminal DoS: the native selector reaches final `CloseSlab` and
fails with `InstructionError(2, Custom(10))`, i.e. `InvalidMint` from the burn
path, before the terminal tombstone can commit.

The production change is confined to `handle_close_slab`. At `ReadyToClose`, a
native primary mint selects the canonical asset-0 insurance beneficiary. The
native branch validates an additional writable, canonical, unencumbered insurance
ATA and transfers exactly the retired booked amount to it. This follows the
canonical insurance disposition requirement in `scripts/loop.md`; the cleanup
administrator does not acquire booked residue merely by signing. Classic SPL
still burns exactly that amount. Engine gates, instruction data, scan/expiry/
recredit outcomes, external-surplus routing and tombstone/rent rules are unchanged.

The additional native destination follows the existing writable primary mint:
account index 7 on a single-quote close, index 9 on a dual-quote close. It is
required only when native booked residue is nonzero. The beneficiary does not
sign or need a funded System wallet. A keeper can recreate its ATA in the same
transaction. The administrator is still required for normalization and final
close; this is not permissionless administrative retirement.

## Product and oracle

Two exact selectors under
`inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup`:

```text
v16_program_native_booked_residue_escheats_after_fee_recredit_completion
v16_program_classic_booked_residue_burn_control_preserves_paid_claims
```

The native Cartesian product has 12 histories:

| Axis | Values |
| --- | --- |
| Retained provider principal at expiry | 74, 101 atoms |
| Spent insurance recovered | 73 atoms |
| Claim-free booked residue | 1, 28 atoms |
| External vault donation | zero; 19 unsynced lamports; 19 synchronized lamports |
| Insurance custody at cleanup | funded existing ATA; redeemed/absent ATA and absent wallet |

Two classic SPL controls cover the same residue amounts without native donations
or custody redemption. These guard the unchanged burn/supply branch of the patch,
rather than supplying another payment-order product.

The reused public fixture settles user payouts exactly to 0 and 2,051,699 atoms,
then deletes their empty portfolios. Public history has earned 657 provider-fee
atoms and 218 insurance-fee atoms, consumed 73 insurance atoms and retained one
source-principal atom. Five unsigned reserve payments and one signed expiry
normalization execute one representative word: source principal, provider
principal, available insurance, expiry, recovered insurance, provider earnings.
The book checks the separate stock classes and all recipients at every prefix;
stock/encumbrance censuses, authority profiles/sequences, mint images and lazy
provider-ledger totals accompany those checks. Its economic rank reaches zero
while physical/logical custody is still exactly 1 or 28 atoms.

Provider/operator wallets are drained publicly and their keys are dropped before
reserve payments. The beneficiary signs only voluntary old native custody
redemption/departure in the missing-custody branch; all reserve keys are dropped
before donation, repair or final close. Existing paid wSOL is redeemed for exactly
249 atoms plus ATA rent, then the wallet balance moves to a separately tracked
departure account. The final cleanup must not pay that recovered claim again.
Keeper-funded ATA recreation restores custody with no wallet or role signature.

The terminal oracle derives its amounts from the input book, never from an
observed engine debit. For booked residue `R`, donation `D` and synchronized
donation `S`:

```text
native insurance ATA increase = R
administrator wrapped surplus = S
administrator close lamports = slab rent excess + vault rent + D - S
native mint supply change = 0
classic mint supply decrease = R
paid claim custody + prior redeemed claims + classic burned residue = input supply
```

Native administrator redemption then returns `S` plus its own ATA rent, making
the donation's total SOL disposition independent of sync timing. Native booked
residue remains in insurance wSOL custody; beneficiary-free redemption of that
new value is not claimed. Full expected token Account images, exact tombstone
bytes/rent, absent vaults, framed wallets, mint and paid ledger bind each result.
Every instrumented transaction also conserves all tracked/compiled lamports
minus the exact signature fees and checks payer-funded rent explicitly.

Each native history rejects a substituted administrator residue destination with
complete rollback. Every history first executes the identical successful final
close/repair prefix followed by an unsigned administrative suffix; logs require
the close prefix to have succeeded, and all Accounts roll back except exact
signature fees. The unchanged prefix then commits. These are integrated safety
controls for the new balanced transition, not separate rejection-only probes.
All instrumented calls enforce 1,232-byte transactions and a measured 300,000-CU
ceiling, below the reused helper's 1,200,000-CU runtime budget.

## Rows and limits

Row 418 is COVERED and marked as an independent discovery because the same public
selector fails on the current pre-fix SBF and passes after the wrapper correction.
Rows 420/421/433 remain OPEN. INV-018/021/025 receive exact quote/custody/rent
evidence; INV-069/070 receive the bounded native retirement correction and
regression; INV-073/078/081 receive the finite completion/cleanup route. Existing
INV-024/027/063/067 setup/oracles are reused, not promoted to new general proofs.

Limits: one asset, integral fixed claims, one completion word, full fixed recredit,
exact expiry, one custody disruption, retained administrator and rent-funded
keeper. Arbitrary role succession/coalescence, absent-admin retirement, partial
recredit products, active claims/receipts, Recovery, dual-quote nonzero native
booked cleanup, dense scans/maximum shapes, frozen/delegated/multisig custody,
Token-2022, arbitrary donations and generic reachability remain outside scope.
The native genesis mint and initial signer SOL are the existing LiteSVM fixture;
all economic and custody transitions use public instructions. No injected market,
portfolio or token state creates the tested condition.

## Artifacts and validation

The user-supplied SBF is read-only at
`/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so`:
`79b298706ca7ffab09a017e5f37c4c0fc43d4e6292c092d126f4fbe8dff1b673`.
`git diff 0dbac7d2 e5273925 -- src Cargo.toml Cargo.lock` is empty, binding its
source provenance to the base. The current PR135 pre-fix control used for the red
native selector is `/dev/shm/percolator-pr426-build.0f9JkJ/target/deploy/percolator_prog.so`.

The corrected SBF is built locally with default features, platform-tools v1.52,
locked/offline dependencies and unchanged engine
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Corrected SBF SHA-256:
`bb70d15a926a3ae5753a8eaf882f97baa52d62563a57b803332c8a92973300e5`.
The native regression fails on the pre-fix artifact and passes on this corrected artifact. No matcher is
needed. Host and SBF intermediates use a private 8-GiB executable tmpfs inside
the worktree because the root filesystem has little free space.

The first execution of both new exact selectors passed, 2/2 in 14.83 seconds:

| Quote | Histories/retirements | ATA repairs | Exact rollbacks | Peak CU |
| --- | ---: | ---: | ---: | ---: |
| Native | 12 | 6 | 24 | 227,930 |
| Classic SPL | 2 | 0 | 2 | 221,841 |

The peak includes instrumented reserve completion, expiry, custody departure,
donation/sync, cleanup rejection/retry and redemption; earlier fixture trading,
marks and user exits are excluded. Each history takes six completion calls and
one successful final close transaction (including ATA repair where needed).

All seven adjacent exact selectors pass on both the supplied SBF (28.49 seconds)
and corrected SBF (28.48 seconds). They cover the original fee/recredit fixture,
Scopes B/X, native sync reclassification, dual-quote terminal disposition, real
obligations blocking close, and native quote roundtrip. The highest separately
reported adjacent peak is 381,292 CU in the original fee/recredit test, within
that selector's existing bound. CU variation includes randomized account keys.
All four `v16_program_fuzz_regressions` metadata gates pass, 4/4 in 0.01 seconds.
There are no ignored or zero-match test passes.

`cargo fmt --all -- --check`, `git diff --check`, staged whitespace checks and
`git show --format= --check HEAD` pass. Existing unused-support and Solana
future-compatibility warnings remain. No marginal exploratory probes were added;
only the two final conformance selectors are retained.

Exact commands, from this worktree (exports below replace equivalent per-command
environment assignments used during execution):

```sh
mkdir -p target/host target/validation target/deploy
sudo -n mount -t tmpfs -o size=8G tmpfs "$PWD/target/host"
export CARGO_TARGET_DIR="$PWD/target/host"
export TMPDIR="$CARGO_TARGET_DIR"
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PERCOLATOR_FUZZ_SBF=/run/percolator-pr135-scope-w-20260913/target/deploy/percolator_prog.so

git diff 0dbac7d2 e5273925 -- src Cargo.toml Cargo.lock
sha256sum "$PERCOLATOR_FUZZ_SBF"
CARGO_TARGET_DIR="$PWD/target/host/sbf" RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc \
  cargo build-sbf --tools-version v1.52 --no-rustup-override --offline \
  --sbf-out-dir "$PWD/target/deploy" -- --locked
sha256sum target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run

PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so" \
  cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_native_booked_residue_escheats_after_fee_recredit_completion \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::terminal_progress_product::native_booked_residue_cleanup::v16_program_classic_booked_residue_burn_control_preserves_paid_claims

scope_o_adjacent=(
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_recredit_fee_partition::v16_program_terminal_recredit_preserves_earned_fee_partition_across_payout_orders
  inv_073_no_permanent_user_lock::absent_insurer_spent_retirement::native_recredit_custody::v16_program_recredited_insurance_recreates_native_custody_without_role_signatures
  inv_073_no_permanent_user_lock::dual_quote_reserve_progress::native_residue_disposition::v16_program_native_residue_and_recreated_reserves_reconcile_quote_variant_retirement
  inv_070_zero_unattributed_terminal_residue_and_close_slab::terminal_native_reclassification::v16_program_native_sync_after_terminal_prefix_reclassifies_only_external_surplus
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_dual_quote_terminal_history_classifies_stock_and_exact_tombstone_rent
  inv_070_zero_unattributed_terminal_residue_and_close_slab::v16_program_close_slab_rejects_until_market_has_zero_terminal_residue
  inv_081_success_state_validity_over_complete_public_routes::v16_program_native_quote_roundtrip_preserves_lamports_rent_and_unsynced_value
)
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 "${scope_o_adjacent[@]}"
PERCOLATOR_FUZZ_SBF="$PWD/target/deploy/percolator_prog.so" \
  cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=2 "${scope_o_adjacent[@]}"

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

Logs remain under `target/validation/`; the corrected artifact remains under
`target/deploy/`. The private host mount is unmounted after validation to release
intermediates. No unfiltered suite, engine/Kani proof or pre-fix native failure
experiment is included.
