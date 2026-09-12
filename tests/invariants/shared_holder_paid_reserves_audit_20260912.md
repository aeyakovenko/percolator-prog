# Shared holders with paid reserve history

Base: `f70a5d4e56dbce3b96c3d6cfdb67ad5db3fda944`. Worktree:
`/tmp/percolator-astra-terminal-progress-reserves-20260912`; branch:
`codex/astra-terminal-progress-reserves-rows420-421-433-20260912`.
The original checkout lacks the invariant directory and has unresolved changes;
the worktree uses the local invariant-enabled baseline. Existing checkouts were
not modified. No GitHub PR, issue, remote branch, or sealed holdout was consulted.

## Relation and Boundaries

The new INV-073 child is
`cu/inv_073_shared_holder_paid_reserves.rs`, mounted as
`inv_073_no_permanent_user_lock::shared_holder_paid_reserves`.
Its exact selector is
`v16_program_absent_shared_holders_keep_paid_reserves_separate_from_public_user_exit`.

Four public LiteSVM histories cross both claimant orders and `CloseResolved` /
`PermissionlessCrank`. The two exposed portfolio owners are also the backing
provider and insurance beneficiary. A distinct live insurance operator and
administrator complete the role set. System, SPL, ATA and wrapper instructions
construct all accounts and economic state. Harness inputs are program loading,
initial signer SOL, Clock, and blockhashes; no economic account bytes are installed.

Input principal is 52,502/2,000,000 user atoms, 100,000 provider atoms, and 31
insurance atoms. A 1,000-lot trade moves from 100 to 105; a 50-lot extension
creates 875 utilization-fee atoms at the signed 3,333-bps rate. While Live, the
provider withdraws 17 principal and 19 earned-fee atoms, and the separate
operator withdraws 7 insurance atoms. The provider spends its entire 36-atom
payment into the operator's SPL account. Thus both user destinations are empty
again, although the provider has an executed, nonzero earnings ledger.

Both holder keypairs and the operator keypair are dropped; the administrator
also supplies no signature. Every subsequent transaction has only the unrelated
keeper's signature. Authenticated slot 6
permits stale resolution. Slot 10 refuses both unsigned user exits; at slot 11,
each successful user step strictly lowers `(remaining legs, unpaid entitlement)`.
Loser-first histories finish in two calls; winner-first histories need three.
The independent owner oracle is 56,627/1,995,000 atoms, excluding all previously
paid reserve value. Both payout aliases reach the same endpoint.

Before each committed user step, three bundles execute that same step and then
reject an unsigned principal, earned-fee, or insurance withdrawal, or hit the
equivalent live-lock gate first. All tracked and compiled Accounts roll back
exactly, including metadata, lamports and account presence; the payer loses only
the calculated transaction signature fee. Error indices and completed wrapper/SPL
counts establish the successful prefix. Across the four histories there are 66
exact rejections, including 24 after an actual SPL user payment. Completed-user
retries and unsigned mechanical portfolio deletion are separate rejection controls.

Each paid reserve prefix has input-derived stock and token checks. Throughout
the keeper continuation, the mint, wallets, operator's 43-atom token Account,
provider ledger, role profile and control sequences remain exact. Custody plus
all destinations equals the fixed 2,152,533-atom supply. Final user capital, PnL
and OI are zero; two economically empty portfolio accounts remain. The final
100,863 vault atoms are exactly 99,983 provider principal, 856 unpaid utilization
fees and 24 beneficiary insurance atoms. Terminal unsigned reserve controls
preserve all three claims.

This is bounded INV-073/018/021/024/027/064/067/071/080/081/082 evidence. It does
not establish beneficiary-independent reserve withdrawal, reserve forfeiture,
mechanical deletion, asset/slab retirement, or generic reachability. Rows
**420, 421 and 433 remain OPEN**, and every existing TSV data row is unchanged.
No production invariant violation was observed in this witness.

## Existing Coverage and Discarded Probes

| Existing owner | Distinct new relation |
| --- | --- |
| INV-073 public reserve / DrainOnly exits | The same holders own exposed user portfolios and partially paid reserve claims; paid provider principal and earnings have already been spent. |
| `inv_073_absent_insurer_spent_retirement.rs` | Insurance remains positive and unspent by bankruptcy. No exhaustion, recredit, expiry or retirement claim is added. |
| `inv_067_terminal_provider_insurance_retries.rs` | That absent-provider history has separate user owners and a signing beneficiary. Here both reserve holders stop signing and retain mixed unpaid claims after paid live prefixes. |
| `inv_082_terminal_custody_alternate.rs` | That shared-role history has flat principal and restricted destinations. Here real exposure, PnL, earned fees and paid/spent stock compose through intact custody. |
| `inv_024_terminal_reserve_destination_recovery.rs` | No custody reconstruction or rent repair is performed here. |
| `inv_024_terminal_earnings_roundtrip.rs` | No provider succession, authority roundtrip, retained withdrawal consumption, or replay-across-replenishment claim is added. |

Discarded before execution: duplicate expiry/exhaustion retirement, reserve ATA
repair and provider-roundtrip selectors; receipt reclassification, pending-loss,
capacity, fractional-carry and incomplete-observation products belong to the
excluded neighboring rows. Partial terminal reserve payments with still
materialized portfolios were also excluded because the public withdrawal
precondition requires their deletion; the new paid prefix occurs while Live.

The first runtime draft incorrectly used the admin-only fee-policy helper after
assigning the insurance authority to the holder. Its `Unauthorized` setup rejection
was corrected by using that holder's real signature. This did not reach the
measured history and is not a production finding or liveness counterexample.
Two compile-only corrections used the existing portfolio-close builder and saved
the rejection flag before consuming its error. No failed economic oracle was
discarded or weakened.

## Validation

A private, independent copy of the shared build cache lives at
`/run/user/1001/astra-terminal-progress-reserves-target`. The default-feature
wrapper SBF was rebuilt offline in this worktree with platform-tools v1.52 and
engine `394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Program SHA-256:
`d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
No matcher fixture is required. The measured live reserve prefixes, SPL spend,
resolution, successful terminal steps and rejections peak at **555,796 CU** on
the integrated artifact, below the explicit 600,000-CU transaction ceiling;
initial funding/trades are outside this reported peak.

Exact commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/run/user/1001/astra-terminal-progress-reserves-target
export PERCOLATOR_FUZZ_SBF="$CARGO_TARGET_DIR/deploy/percolator_prog.so"
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export TMPDIR=/run/user/1001
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline --sbf-out-dir "$CARGO_TARGET_DIR/deploy" -- --locked
cargo test --locked --offline --test v16_cu -- --exact --nocapture --test-threads=1 inv_073_no_permanent_user_lock::shared_holder_paid_reserves::v16_program_absent_shared_holders_keep_paid_reserves_separate_from_public_user_exit inv_073_no_permanent_user_lock::v16_program_terminal_provider_earnings_and_lazy_ledger_reach_exact_slab_close inv_067_terminal_payout_completeness_and_exact_once_settlement::provider_insurance_retries::v16_program_absent_provider_preserves_user_order_and_operator_free_insurance_exit
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The final focused run passes all three exact selectors (3/3, 3.96s), including
the four-world increment, the cooperative earnings/retirement control and the
separate-owner absent-provider control. The invariant charter/index passes 1/1
after the README and coverage-note update. Repository-wide formatting and Git
whitespace checks pass. Existing unused-support and Solana future-compatibility
warnings remain. No unfiltered suite or vulnerable-pin experiment is claimed.
