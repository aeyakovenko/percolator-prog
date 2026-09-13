# Retained fee consent through grant expiry

Owner: `stateful/inv_014_retained_fee_expiry.rs`, mounted as a child of
`stateful/inv_014_retained_permitted_policy_history.rs` to reuse its existing
fee arithmetic, budget/custody oracle, signature construction, full-Account frame
and successful-transaction checks.

Exact new selector, in `v16_program_stateful_fuzz`:

```text
inv_014_delayed_policy_and_policy_epoch_safety::retained_permitted_policy_history::retained_fee_expiry::v16_program_retained_fee_prefix_rolls_back_at_grant_expiry_before_bilateral_retry
```

## Public history and oracle

Sixteen histories cross single/batch CPI for both independent owner pairs,
both trade directions, and Clock slot 4/5 against a grant expiring at slot 4.
They use the existing `V16Svm` valid-account construction: zeroed program-owned
allocation, valid fixed-supply SPL custody and public wrapper initialization,
deposits and authenticated matcher setup. Signer SOL and executable programs are
harness inputs. No initialized economic Account is edited out of band. Clock
advances through LiteSVM; all economic/control actions use public instructions.

At slot 3, two bundled CPI requests, the first CPI alone and a bilateral
alternative for the second pair are signed, packet checked and simulated. Their
complete wire transactions predate market-authority succession and repricing from
19 to the signed 37-bps rate. The unchanged bundle also simulates successfully at
37 bps in the last live slot. LP grants and taker consent have the same cap;
this does not independently retest the single-CPI taker-only fee guard.

At exact/late expiry, the first CPI succeeds and the second returns `Unauthorized`.
The independently pre-signed retry has the same disposition. A third transaction
first lowers the policy to 7 bps, then completes the first paid CPI before expiry
rejects the second. Exact error indices and successful wrapper/matcher log counts
establish the completed prefix. Every compiled and tracked complete Account is
restored, including policy, both portfolios, context, SPL balances and passive
accounts. Only the separate fee payer loses its exact runtime signature fee.

The retained bilateral alternative then opens the expired pair without a renewed
grant, and the retained first CPI opens its unaffected pair. Fresh opposite
single/batch closes charge 37 bps again. The input-only two-ceiling fee formula
checks capital, zero PnL, positions, OI, side-local insurance, fixed mint supply,
source/destination SPL balances and stock/encumbrance censuses at each checkpoint.
All five owners withdraw their full net capital and the assigned beneficiary
withdraws both assets' fee earnings. Every complete token-account endpoint agrees
across all sixteen histories, and primary custody finishes empty.

Each opening/closing fill charges 371 atoms per owner in pair 0 and 1,111 in
pair 1. Their complete owner payouts are respectively 99,999,258 and 99,997,778
atoms per owner. The beneficiary receives its unchanged 2,000,000,000 principal
plus 5,928 fee atoms. The primary vault ends at zero; the complete fixed supply
is still 4,500,000,000 atoms, including untouched source and foreign custody.

## Coverage boundary

| Existing coverage | Net-new composition here |
| --- | --- |
| Retained backing fee caps | No backing fees here; LP expiry terminates a bundle after another pair has paid a base fee. |
| Retained redirect entitlement | No redirect policy here; exact/late expiry and retained bilateral fallback replace paid-recipient stock exhaustion. |
| Used capability succession | No used object or scope replacement here; Clock expires an otherwise unchanged owner grant after policy-authority succession. |
| Retained single-CPI taker cap | Both caps agree and current fees fit; the new guard boundary is expiry after a successful fee-bearing prefix. |
| Retained fee bundle route product | Policy remains within both fee envelopes; expiry supplies the suffix rejection, with an unchanged bilateral fallback signed before handoff. |
| Retained permitted policy history | Reuses its budget/frame; adds expiry, repeated refusal and a bilateral alternate route. |
| INV-012 retained scope admin/tuple/expiry product | Existing zero-fee expiry coverage does not compose a paid sibling prefix, policy rollback and exact fee-recipient payout. |

This adds bounded INV-010/011/014/024/036/047/080/081 evidence, with INV-012's
expiry rule as a precondition. **Rows 411 and 432 remain OPEN.** No generic
history generator, arbitrary-sequence oracle, vulnerable-pin red/green run,
production fix or invariant-status promotion is claimed. Multi-leg batches,
partial fills, moving prices, backing/funding/maintenance fees, renewed grants,
alternate token rails and terminal retirement remain outside this increment.

Discarded directions: repeating retained backing caps, paid redirect recipients,
used capability succession or the taker-only single-CPI cap would duplicate
existing coverage. Those probes were not added or run. Discarded setup attempt:
the first setup placed the initial policy update after transferring
the insurance-operator role but used the old admin helper. The program correctly
returned `Unauthorized` before the retained history began. Moving that initial
policy update before role transfer fixed the fixture; it supplies no coverage
or finding. No behavioral probe was discarded after reaching the intended history.

## Validation

Isolated worktree:
`/tmp/percolator-astra-retained-fee-consent-rows411-432-20260912`.
Branch: `codex/astra-retained-fee-consent-rows411-432-20260912`.
Base: local committed `f70a5d4e56dbce3b96c3d6cfdb67ad5db3fda944`; the main
checkout and local `main` lack the requested invariant files. No existing worker
worktree was read or changed. No GitHub PR, issue, branch or sealed holdout was
inspected. Only committed local public tests and documentation informed the case.

Fresh locked/offline default-feature wrapper and authenticated-matcher builds
use platform-tools v1.52 and pinned engine
`394fd0bf2cb7d73df425eb3754dc3be1a0c44336`. Artifact SHA-256:

- Wrapper: `d5f2d3d2c35842aab0979ab24fed415fe36b2b93ed6cc80998fee839ae76343f`.
- Authenticated matcher: `50e532267926e180f013200c1799e26127dd23dc150866cffd491424629ddf93`.

Build/test artifacts use the private
`/run/user/1001/astra-fee-rows411-432-20260912` directory because root and
`/dev/shm` had less than 1 GiB available.

The new exact selector passes: **16 histories, 80 non-consuming simulations,
48 complete-Account rollbacks, 208 measured successful transactions**, including
80 owner and 32 insurance payouts. Peak measured success is **150,488 CU**;
peak rejection is **153,592 CU**, below the 1,400,000 ceiling. Setup calls made
through the existing fixture helpers are not included in these suffix CU counts.
No production invariant violation was observed. Existing dead-code warnings and
the `solana-client v1.18.26` future-compatibility warning remain.

Reproduction commands from the isolated worktree:

```sh
export CARGO_TARGET_DIR=/run/user/1001/astra-fee-rows411-432-20260912/target
export TMPDIR=/run/user/1001/astra-fee-rows411-432-20260912/tmp
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --tools-version v1.52 --no-rustup-override --offline -- --locked
RUSTC=/home/anatoly/.cache/solana/v1.52/platform-tools/rust/bin/rustc cargo build-sbf --manifest-path tests/fixtures/auth_matcher/Cargo.toml --sbf-out-dir tests/fixtures/auth_matcher/target/deploy --tools-version v1.52 --no-rustup-override --offline -- --locked
cargo test --locked --offline --test v16_program_stateful_fuzz inv_014_delayed_policy_and_policy_epoch_safety::retained_permitted_policy_history::retained_fee_expiry::v16_program_retained_fee_prefix_rolls_back_at_grant_expiry_before_bilateral_retry -- --exact --nocapture
cargo test --locked --offline --test v16_program_stateful_fuzz inv_014_delayed_policy_and_policy_epoch_safety::retained_permitted_policy_history::v16_program_retained_cpi_owner_budgets_ignore_permitted_policy_detours -- --exact --nocapture
cargo test --locked --offline --test v16_program_fuzz_regressions inv_079_public_reachability_evidence::v16_invariant_charter_and_index_are_complete -- --exact --nocapture
cargo fmt --all -- --check
git diff --check
git diff --cached --check
git show --format= --check HEAD
```

The shared-helper control passes its 32 histories, 64 simulations and 64 exact
rollbacks (544 measured successes, peak 150,081 CU). The invariant charter/index
passes 1/1. Repository formatting, working/staged whitespace checks and committed
diff checks pass. Both reopening data rows still say OPEN; only comments were
added to the ledger. No broad suite was run.
