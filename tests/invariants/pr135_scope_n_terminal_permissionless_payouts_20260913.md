# PR135 Scope N: generated terminal reserve wallet availability

Base: freshly fetched `origin/codex/astra-open-holdout-ledger-20260912`,
`5a922618a70734e4a6db25095aadb2b94b6d9702`. Branch:
`codex/pr135-scope-n-terminal-permissionless-payouts-20260913`.
All source edits and build artifacts belong to the new worktree, initially
`/tmp/percolator-scope-n-terminal-permissionless-20260913`, subsequently moved to
`/dev/shm/percolator-scope-n-terminal-permissionless-20260913` for disk space.
Neither requested protected checkout was edited. Shared Git administration was
used only to fetch the requested base, create/repair this worktree and commit.
Only the requested base's invariant documents, production code and existing tests
were consulted. No open PR branch, diff or test source was inspected or copied.
The three row numbers are coverage labels, not implementation specifications.

## Added evidence

Primary owner: [INV-073 probe](cu/inv_073_generated_reserve_wallets.rs), mounted
beside the existing public earnings fixture and invoked by an INV-073 selector.
The existing classic fixture remains the default. Its optional native path uses
the existing public native-market constructor with the same risk parameters and
System transfer/SyncNative funding in place of public classic minting.

The matrix has 128 histories: four seeds `0x4e00..0x4e04`, two primary quote types,
four independently selected provider/insurer wallet states, and four expiry
frontiers. Each seed chooses a 1,000..9,000-exclusive insurance fee share in bps,
nonzero partial payments for three claims, and a shuffled word containing two
principal payments, two fee payments and an insurance prefix. An insurance tail
ends the word, preserving a real claim across every normalization frontier.
Expiry occurs before event 0, 2 or 4, or does not occur during the word. The
authenticated Clock moves to slot 100 or 101, selected by seed parity.

The public fixture supplies 100,000 provider atoms, 31 insurance atoms and two
funded traders. A 100-to-105 authenticated mark and a backed 50-lot admission
produce an independently computed 875-atom utilization fee. Insurance receives
`floor(875 * share_bps / 10000)`; provider earnings receive the complementary
amount. Both traders settle their original exact entitlements before the new
reserve schedule, retaining the 5,000-atom consumed backing/receivable history.
Those counters are checked as history, never as additional payout entitlement.

Each selected missing-wallet case publicly closes empty ATA custody and drains
the holder's entire SOL balance. The operator is always drained. All three role
keypairs are then dropped, including in wallet-present controls. An empty wallet
may be absent or the exact zero-lamport, empty System account retained by LiteSVM.
The keeper recreates required custody in the same transaction as the first
payment. Native custody recreation wraps no new principal. A keeper-funded
program-owned earnings ledger starts uninitialized; its first earnings payment
initializes telemetry without a provider signature or earlier signed ledger sync.

An input-derived claim book checks principal, provider earnings and insurance
separately at every new payment/normalization prefix. Quote type and wallet
availability must produce identical cumulative entitlements for each generated
word/expiry frontier. Each payment decreases unpaid claim rank by exactly its
requested amount. Expiry removes only remaining principal from that rank; fee
and insurance entitlements survive, and expired principal remains raw custody.
No engine progress selector chooses the continuation.

Before every payment, an identical creation/payment prefix is followed by an
unsigned administrative CloseSlab. The exact ExpectedSigner error and completed
wrapper/ATA prefix logs are checked. Every compiled/tracked Account rolls back,
including SPL bytes, native lamports, missing custody, market, ledger and paid
prefixes; only the calculated payer signature fee changes. The unchanged prefix
then commits with the keeper as its sole signer. Successful calls frame every
unrelated Account and charge exact creation rent. Full expected token Account
images, mint frames, authority profile/sequences, shape, stock and encumbrance
censuses accompany the claim book.

Classic endpoints burn only expired claim-free principal and retire with exact
tombstone/vault rent accounting. Native endpoints with zero remaining raw stock
also retire. Following the existing quote-capacity coverage boundary, native
endpoints retaining expired principal stop after all beneficiary claims are
paid and the exact raw residue is checked. Native principal burning is not
attempted or certified. Administrative normalization and final retirement use
the retained market authority; earlier empty portfolio deletion uses user keys.

## Distinction and limits

Scope K covers dense solvent users, unspent insurance and fresh principal with
zero fees. Scope L covers unspent native insurance, multisig redemption and
recreation with no provider earnings or expiry. Scope J covers mixed creditor
and debtor receipt settlement. Scopes G/H/I cover shared source capacity, carry
entitlement and grant binding. None owns this generated fee allocation/payment
word crossed with native provider earnings and independently drained reserve
wallets. The existing INV-070 native reclassification, INV-073 fixed reserve
orders/custody replacement/native principal redemption, and INV-077 quote variants
remain adjacent controls and retain their distinct coverage claims.

Rows 420/421/433 remain OPEN, with their historical inventory entries unchanged.
INV-073 is the primary bounded completion claim. INV-018/021, INV-026/063/067/069/070,
and INV-080/081 receive custody, classification, retirement-subset and transaction
evidence. INV-027/032 retain the fixture's settled senior payouts and consumed
history frames; this is not a new active-seniority or lien-lifecycle matrix.
INV-071/078/082 receive a constructive finite reserve suffix under the named
authority/time/rent assumptions, not generic permissionless recovery or closure.
No invariant status, proof classification, production source or dependency changes.

Limits: one asset/domain for provider backing, fixed public exposure history,
four sampled words with one mandatory final insurance event, integral solvent
claims, no shrinking/exhaustive graph, no active receipts/pending losses at reserve
payment, no insurance spend/recredit, Recovery, native SOL redemption, dual quote
switching, Token-2022, custody freeze/reassignment, arbitrary role succession,
maximum market shape, absent-admin normalization or native-residue retirement.
The market must be resolved with user claims settled and portfolios deleted.

## Validation

The locked engine is `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. A fresh
default-feature SBF build used platform-tools v1.52 in this worktree. SHA-256:
`71833878f8d52373e501e70ed21d88495051695f2533b4c55362ae90e737aa9d`.
No matcher is required by these direct public routes. SPL/ATA artifacts come
from the installed LiteSVM dependency fixture paths, as in existing tests.

The new exact selector passes **1/1 in 72.41 seconds**: 128 histories,
648 keeper-only payments, 648 complete Account rollbacks, 128 custody repairs,
80 principal normalizations, 104 earnings payments after principal expiry,
56 payment prefixes with zero available principal and surviving fees,
88 exact retirements and 40 native raw-residue endpoints. Peak measured
consumption is **258,035 CU**, below the asserted 600,000-CU ceiling. The reused
transaction helper sets a 1,200,000-CU runtime budget. The exact list command
selects one test, with no ignored tests.

Four adjacent exact selectors pass **4/4 in 14.65 seconds**: classic earned-fee
expiry, public reserve dispositions, Scope L native multisig insurance, and the
native quote round trip. All four INV-079 metadata gates pass **4/4**, including
the reopening inventory projection. Formatting, unstaged/staged whitespace and
the committed `git show --format= --check HEAD` check pass. The existing unused
support warnings and Solana future-compatibility warning remain.

Initial build/provenance commands:

```sh
git fetch origin refs/heads/codex/astra-open-holdout-ledger-20260912:refs/remotes/origin/codex/astra-open-holdout-ledger-20260912
git worktree add -b codex/pr135-scope-n-terminal-permissionless-payouts-20260913 /tmp/percolator-scope-n-terminal-permissionless-20260913 origin/codex/astra-open-holdout-ledger-20260912
cd /tmp/percolator-scope-n-terminal-permissionless-20260913
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_TARGET_DIR=$PWD/target/sbf-build RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$PWD/target/deploy" -- --locked
sha256sum target/deploy/percolator_prog.so
cargo clean --target-dir target/sbf-build
```

The two space-limited compilation attempts and worktree relocation were:

```sh
# In the original worktree, with the build-profile exports above:
export CARGO_TARGET_DIR=$PWD/target/host
export PERCOLATOR_FUZZ_SBF=$PWD/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
cd /tmp
mv /tmp/percolator-scope-n-terminal-permissionless-20260913 /dev/shm/percolator-scope-n-terminal-permissionless-20260913
cd /dev/shm/percolator-scope-n-terminal-permissionless-20260913
git worktree repair /dev/shm/percolator-scope-n-terminal-permissionless-20260913
export CARGO_TARGET_DIR=$PWD/target/host
export PERCOLATOR_FUZZ_SBF=$PWD/target/deploy/percolator_prog.so
cargo test --locked --offline --test v16_cu --test v16_program_fuzz_regressions --no-run
```

Final commands, run sequentially from the relocated worktree:

```sh
cd /dev/shm/percolator-scope-n-terminal-permissionless-20260913
export CARGO_TARGET_DIR=$PWD/target/host
export PERCOLATOR_FUZZ_SBF=$PWD/target/deploy/percolator_prog.so
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::v16_program_generated_reserve_wallet_absence_preserves_fee_claims_across_expiry -- --exact --list
cargo test --locked --offline --test v16_cu inv_073_no_permanent_user_lock::v16_program_generated_reserve_wallet_absence_preserves_fee_claims_across_expiry -- --exact --nocapture --test-threads=1
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 \
  inv_073_no_permanent_user_lock::v16_program_terminal_public_reserve_disposition_preserves_value_across_orders \
  inv_024_attributed_quote_value_conservation::terminal_earnings_succession::terminal_earnings_expiry::v16_program_terminal_expiry_preserves_earned_fees_and_bounded_signed_disposal \
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
git show --format= --check HEAD
```

Two host build attempts hit shared filesystem exhaustion before executing tests:
the original root-filesystem build and a two-target build after relocation.
Only this worktree was moved and its Git administrative link repaired. Its own
358 MiB SBF intermediates were removed after preserving the deployable artifact.
Host checks were then serialized. Development corrected two fixture/oracle
assumptions: LiteSVM's retained empty System account after draining, and native
rent refunds after all wrapped payments had already removed the initial balance.
Neither was a production invariant failure. No production fix or bug reproducer
is claimed. No unfiltered suite or engine proof was run.
